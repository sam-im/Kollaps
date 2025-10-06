from setuptools import setup, find_packages

setup(
    name='kollaps',
    version='2.0',
    description='Decentralized network emulator',
    url='https://github.com/miguelammatos/Kollaps.git',
    author='Joao Neves, Paulo Gouveia, Luca Liechti',
    packages=find_packages(include=['kollaps', 'kollaps.*']),
    install_requires=[
        'dnspython',
        'docker',
        'kubernetes',
        'netifaces',
        'ply'
    ],
    include_package_data=True,
    package_data={
        'kollaps.TCAL': ['libTCAL.so'],
        'kollaps.dashboard': [
            'libcommunicationcore.so',
            'static/css/*',
            'static/js/*',
            'templates/*.html'
        ],
    },
    entry_points={
        'console_scripts': [
            'KollapsDeploymentGenerator=kollaps.tools.deploymentGenerators.deploymentGenerator:main',
            'KollapsDashboard=kollaps.dashboard.Dashboard:main',
            'Kollapsbootstrapper=kollaps.bootstrapper.main:main',
            'ThunderstormTranslator=kollaps.tools.Thunderstorm.ThunderstormTranslator:main'
        ],
    },
    zip_safe=False,
)
