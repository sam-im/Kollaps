
from setuptools import setup

setup(name='kollaps',
      version='2.0',
      description='Decentralized network emulator',
      url='https://github.com/miguelammatos/Kollaps.git',
      author='Joao Neves, Paulo Gouveia, Luca Liechti',
      packages=[
          'kollaps',
          'kollaps.TCAL',
          'kollaps.tools',
          'kollaps.tools.deploymentGenerators',
          'kollaps.tools.bootstrapping',
          'kollaps.tools.ThunderStorm',
      ],
      install_requires=[
          'dnspython',
          'docker',
          'kubernetes',  # why is this required
          'netifaces',
          'ply'
      ],
      include_package_data=True,
      package_data={
          'kollaps.TCAL': ['libTCAL.so'],
          'kollaps.dashboard': [
              'static/css/*', 
              'static/js/*',  
              'templates/*.html'
          ],
      },
      entry_points={
          'console_scripts': [
              'KollapsDeploymentGenerator = kollaps.deploymentGenerators.deploymentGenerator:main',
              'KollapsDashboard=kollaps.dashboard.Dashboard:main',
              # 'KollapsLogger=kollaps.Logger:main',  # doesn't exist
              # 'KollapsEmulationManager=kollaps.EmulationManager:main',  # doesn't exist
              'Kollapsbootstrapper=kollaps.tools.bootstrapping.Bootstrapper:main',
              'ThunderstormTranslator=kollaps.tools.Thunderstorm.ThunderstormTranslator:main'],
      },
      zip_safe=False)
      
